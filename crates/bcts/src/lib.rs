#![allow(clippy::needless_lifetimes)]

use rmx::prelude::*;

pub mod input;
pub mod text;
pub mod escapes;
pub mod chunk;
pub mod source_map;
pub mod chunks;
pub mod lexer;
pub mod bracer;
pub mod lines;

pub mod package2;
pub mod package_resolve2;

pub mod module_graph;

pub mod diagnostic;
pub mod parser_util;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
